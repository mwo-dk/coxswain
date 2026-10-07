//! F2's entries: the user's `[[user_menu]]`, or else built-in ones chosen for the system and
//! for what is installed (a terminal, a SHA-256 tool, git), each offered only where it fits.
//! "Add your own command" opens config.toml at `[[user_menu]]`, with an example written first.

use crate::config::{Config, UserCommand, quote};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Where an entry is offered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum When {
    /// The cursor is on a file (not a folder, not `..`).
    File,
    /// The panel's folder is in a git repository.
    Git,
}

/// What F2 offers in `dir` with `file` under the cursor.
pub fn entries(cfg: &Config, dir: &Path, file: Option<&Path>) -> Vec<UserCommand> {
    let list = cfg.user_menu.clone().unwrap_or_else(builtin);
    list.into_iter().filter(|u| fits(u.when, dir, file)).collect()
}

fn fits(when: Option<When>, dir: &Path, file: Option<&Path>) -> bool {
    match when {
        None => true,
        Some(When::File) => file.is_some_and(Path::is_file),
        Some(When::Git) => dir.ancestors().any(|a| a.join(".git").exists()),
    }
}

/// The built-in entries for this system, labels in the current language. What is installed is
/// looked up once.
fn builtin() -> Vec<UserCommand> {
    static FOUND: std::sync::OnceLock<Vec<UserCommand>> = std::sync::OnceLock::new();
    let found = FOUND.get_or_init(|| {
        let os = if crate::termux::active() { "android" } else { std::env::consts::OS };
        defaults(os, &installed, &|v| std::env::var(v).ok().filter(|s| !s.is_empty()))
    });
    found.iter().map(|u| UserCommand { label: crate::t!(&u.label), ..u.clone() }).collect()
}

/// A program on PATH; on Windows also an app alias (Windows Terminal's `wt`), which is not a
/// regular file.
fn installed(program: &str) -> bool {
    crate::tools::which(program).is_some()
        || (cfg!(windows) && std::env::var_os("LOCALAPPDATA").is_some_and(|d| Path::new(&d).join("Microsoft").join("WindowsApps").join(format!("{program}.exe")).symlink_metadata().is_ok()))
}

/// The built-in entries on `os` (`std::env::consts::OS`, or `android` in Termux), with the
/// programs `has` finds and the environment `var`; labels are i18n keys.
fn defaults(os: &str, has: &dyn Fn(&str) -> bool, var: &dyn Fn(&str) -> Option<String>) -> Vec<UserCommand> {
    let entry = |key: &str, label: &str, command: String, wait: bool, when: Option<When>| UserCommand { key: key.into(), label: label.into(), command, wait, when };
    let mut out = vec![];
    if let Some(c) = terminal(os, has, var) {
        out.push(entry("t", "usermenu.terminal", c, false, None));
    }
    if let Some(c) = sha256(os, has) {
        out.push(entry("h", "usermenu.sha256", c, true, Some(When::File)));
    }
    if has("git") {
        out.push(entry("s", "usermenu.git_status", "git status".into(), true, Some(When::Git)));
    }
    out
}

/// Common terminals, in order, for when neither `$TERMINAL` nor `x-terminal-emulator` is there.
const TERMINALS: &[&str] = &[
    "x-terminal-emulator", "foot", "alacritty", "kitty", "wezterm", "ghostty", "konsole", "gnome-terminal", "ptyxis", "kgx", "xfce4-terminal", "mate-terminal", "lxterminal", "qterminal",
    "terminator", "tilix", "urxvt", "st", "xterm",
];

/// A new terminal window in the folder the command runs in, left running on its own.
fn terminal(os: &str, has: &dyn Fn(&str) -> bool, var: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    match os {
        // Termux: the app already is the terminal.
        "android" => None,
        "macos" => Some("open -a Terminal .".into()),
        "windows" => Some(if has("wt") { r#"start "" wt -d ."# } else { r#"start "" powershell"# }.into()),
        _ => {
            // No desktop (a console, ssh): no window to open.
            var("WAYLAND_DISPLAY").or_else(|| var("DISPLAY"))?;
            let own = var("TERMINAL").filter(|t| t.split_whitespace().next().is_some_and(has));
            let prog = own.or_else(|| TERMINALS.iter().find(|t| has(t)).map(|t| t.to_string()))?;
            Some(format!("nohup {prog} >/dev/null 2>&1 &"))
        }
    }
}

/// The file's SHA-256 with the tool the system has.
fn sha256(os: &str, has: &dyn Fn(&str) -> bool) -> Option<String> {
    let tools: &[(&str, &str)] = match os {
        // Get-FileHash would need PowerShell's quoting inside cmd's; certutil takes cmd's.
        "windows" => &[("certutil", "certutil -hashfile %f SHA256")],
        "macos" => &[("shasum", "shasum -a 256 -- %f")],
        "freebsd" | "openbsd" | "netbsd" => &[("sha256", "sha256 -- %f"), ("sha256sum", "sha256sum -- %f"), ("cksum", "cksum -a sha256 -- %f")],
        "illumos" | "solaris" => &[("digest", "digest -v -a sha256 -- %f"), ("sha256sum", "sha256sum -- %f")],
        _ => &[("sha256sum", "sha256sum -- %f"), ("shasum", "shasum -a 256 -- %f")],
    };
    tools.iter().find(|(p, _)| has(p)).map(|(_, c)| c.to_string())
}

/// Make config.toml ready for a new entry, writing the commented example at its end when it
/// has no `[[user_menu]]` yet, and say where: the file and the line.
pub fn prepare() -> Result<(PathBuf, usize), String> {
    let path = Config::path().ok_or_else(|| crate::t!("err.no_config_folder"))?;
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        // Never written over when it cannot be read.
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let (line, new) = with_example(&text, &example());
    if let Some(new) = new {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, new).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok((path, line))
}

/// The line (from 1) of `text`'s user menu, commented out or not; when it has none, `example`
/// is added at the end, the rest kept as it is, and the new text comes back too.
fn with_example(text: &str, example: &str) -> (usize, Option<String>) {
    let menu = |l: &str| {
        let l = l.trim_start_matches(['#', ' ', '\t']);
        l.starts_with("[[user_menu]]") || l.starts_with("user_menu ") || l.starts_with("user_menu=")
    };
    if let Some(i) = text.lines().position(menu) {
        return (i + 1, None);
    }
    let mut new = text.to_string();
    if !new.is_empty() {
        new += if new.ends_with('\n') { "\n" } else { "\n\n" };
    }
    let line = new.lines().count() + 1;
    new += example;
    (line, Some(new))
}

/// The built-in entries of this system as `[[user_menu]]` tables, and one more to start from,
/// all commented out.
fn example() -> String {
    #[derive(Serialize)]
    struct Menu {
        user_menu: Vec<UserCommand>,
    }
    let mut user_menu = builtin();
    user_menu.push(UserCommand { key: "z".into(), label: "Zip the marked files".into(), command: "zip -r marked.zip %s".into(), wait: true, when: None });
    let tables = toml::to_string(&Menu { user_menu }).unwrap_or_default();
    let mut out = String::from(
        "# F2, the user menu. Entries of your own replace the built-in ones, so these are them,
# ready to keep: take the # off the lines of an entry to use it, and add yours after them.
# %f is the file under the cursor, %d the folder, %s the marked files, each shell-quoted.
# wait = true shows the output; when = \"file\" or \"git\" offers an entry only on a file or
# in a git repository.\n#\n",
    );
    for l in tables.lines() {
        out += if l.is_empty() { "#" } else { "# " };
        out += l;
        out.push('\n');
    }
    out
}

/// `editor` opening `file` at `line`: `+line` for the editors known to take it, else the file.
pub fn edit_command(editor: &str, file: &Path, line: usize) -> String {
    let name = editor.split_whitespace().next().and_then(|p| Path::new(p).file_stem()).map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
    let plus = ["vi", "vim", "nvim", "gvim", "view", "nano", "pico", "emacs", "emacsclient", "micro", "mg", "joe", "jed", "ne", "kak", "mcedit"].contains(&name.as_str());
    let at = if plus { format!(" +{line}") } else { String::new() };
    format!("{editor}{at} {}", quote(&file.to_string_lossy()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu(os: &str, programs: &[&str], env: &[(&str, &str)]) -> Vec<(String, String)> {
        let has = |p: &str| programs.contains(&p);
        let var = |v: &str| env.iter().find(|(k, _)| *k == v).map(|(_, x)| x.to_string());
        defaults(os, &has, &var).into_iter().map(|u| (u.key, u.command)).collect()
    }

    fn command(os: &str, programs: &[&str], env: &[(&str, &str)], key: &str) -> Option<String> {
        menu(os, programs, env).into_iter().find(|(k, _)| k == key).map(|(_, c)| c)
    }

    const X: &[(&str, &str)] = &[("DISPLAY", ":0")];

    #[test]
    fn user_menu_terminal_per_os() {
        assert_eq!(command("macos", &[], &[], "t").as_deref(), Some("open -a Terminal ."));
        assert_eq!(command("windows", &["wt"], &[], "t").as_deref(), Some(r#"start "" wt -d ."#));
        assert_eq!(command("windows", &[], &[], "t").as_deref(), Some(r#"start "" powershell"#), "no Windows Terminal: PowerShell");
        assert_eq!(command("linux", &["xterm", "x-terminal-emulator"], X, "t").as_deref(), Some("nohup x-terminal-emulator >/dev/null 2>&1 &"));
        assert_eq!(command("freebsd", &["xterm", "kitty"], X, "t").as_deref(), Some("nohup kitty >/dev/null 2>&1 &"));
        let own = [("WAYLAND_DISPLAY", "wayland-0"), ("TERMINAL", "foot")];
        assert_eq!(command("linux", &["foot", "xterm"], &own, "t").as_deref(), Some("nohup foot >/dev/null 2>&1 &"), "$TERMINAL first");
        let gone = [("DISPLAY", ":0"), ("TERMINAL", "nowhere")];
        assert_eq!(command("openbsd", &["xterm"], &gone, "t").as_deref(), Some("nohup xterm >/dev/null 2>&1 &"), "a $TERMINAL that is not installed is passed over");
        assert_eq!(command("linux", &[], X, "t"), None, "no terminal installed: hidden");
        assert_eq!(command("linux", &["xterm"], &[], "t"), None, "no display: hidden");
        assert_eq!(command("android", &["xterm"], X, "t"), None, "Termux is the terminal");
    }

    #[test]
    fn user_menu_sha256_per_os() {
        assert_eq!(command("freebsd", &["sha256", "sha256sum"], &[], "h").as_deref(), Some("sha256 -- %f"));
        assert_eq!(command("linux", &["sha256sum", "shasum"], &[], "h").as_deref(), Some("sha256sum -- %f"));
        assert_eq!(command("linux", &["shasum"], &[], "h").as_deref(), Some("shasum -a 256 -- %f"));
        assert_eq!(command("netbsd", &["cksum"], &[], "h").as_deref(), Some("cksum -a sha256 -- %f"));
        assert_eq!(command("openbsd", &["sha256", "cksum"], &[], "h").as_deref(), Some("sha256 -- %f"));
        assert_eq!(command("macos", &["shasum"], &[], "h").as_deref(), Some("shasum -a 256 -- %f"));
        assert_eq!(command("windows", &["certutil"], &[], "h").as_deref(), Some("certutil -hashfile %f SHA256"));
        assert_eq!(command("illumos", &["digest"], &[], "h").as_deref(), Some("digest -v -a sha256 -- %f"));
        assert_eq!(command("linux", &[], &[], "h"), None, "no tool: hidden");
    }

    #[test]
    fn user_menu_git_status_needs_git() {
        assert_eq!(command("linux", &["git"], &[], "s").as_deref(), Some("git status"));
        assert_eq!(command("linux", &[], &[], "s"), None);
        let keys: Vec<String> = menu("linux", &["git", "sha256sum", "xterm"], X).into_iter().map(|(k, _)| k).collect();
        assert_eq!(keys, ["t", "h", "s"]);
    }

    /// The defaults of the system the tests run on (CI runs them on each one), where its tool
    /// is on PATH: a build sandbox (Nix) may have none.
    #[test]
    fn user_menu_defaults_here() {
        let here: Vec<(String, String)> = builtin().into_iter().map(|u| (u.key, u.command)).collect();
        let h = here.iter().find(|(k, _)| k == "h").map(|(_, c)| c.as_str());
        let want = match std::env::consts::OS {
            "windows" => Some(("certutil", "certutil -hashfile %f SHA256")),
            "macos" => Some(("shasum", "shasum -a 256 -- %f")),
            "freebsd" | "openbsd" => Some(("sha256", "sha256 -- %f")),
            "linux" if !crate::termux::active() => Some(("sha256sum", "sha256sum -- %f")),
            _ => None,
        };
        if let Some((tool, command)) = want.filter(|(tool, _)| installed(tool)) {
            assert_eq!(h, Some(command), "{tool}");
        }
        if cfg!(target_os = "macos") {
            assert!(here.iter().any(|(_, c)| c == "open -a Terminal ."));
        }
    }

    #[test]
    fn user_menu_fits_where_it_is_offered() {
        let root = std::env::temp_dir().join(format!("coxswain-user-menu-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let dir = &root;
        let file = dir.join("a.txt");
        std::fs::write(&file, "a").unwrap();
        assert!(fits(Some(When::File), dir, Some(&file)));
        assert!(!fits(Some(When::File), dir, Some(dir)), "a folder is not a file");
        assert!(!fits(Some(When::File), dir, None));
        let sub = dir.join("src");
        std::fs::create_dir(&sub).unwrap();
        assert!(!fits(Some(When::Git), &sub, None));
        std::fs::create_dir(dir.join(".git")).unwrap();
        assert!(fits(Some(When::Git), &sub, None), "anywhere inside the repository");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn user_menu_own_entries_replace_the_builtin_ones() {
        let cfg = Config::parse("[[user_menu]]\nkey = \"x\"\nlabel = \"X\"\ncommand = \"x\"\n").unwrap();
        let got = entries(&cfg, Path::new("/"), None);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].key, "x");
        let none = Config::parse("user_menu = []\n").unwrap();
        assert!(entries(&none, Path::new("/"), None).is_empty(), "an empty list of one's own stays empty");
        assert!(Config::parse("").unwrap().user_menu.is_none());
        let on_file = Config::parse("[[user_menu]]\nkey = \"x\"\nlabel = \"X\"\ncommand = \"x\"\nwhen = \"file\"\n").unwrap();
        assert!(entries(&on_file, Path::new("/"), None).is_empty());
    }

    #[test]
    fn user_menu_example_is_added_once_and_keeps_the_rest() {
        let ex = "# [[user_menu]]\n# key = \"z\"\n";
        let text = "# my comment\ntheme = \"nc\" # mine\n";
        let (line, new) = with_example(text, ex);
        let new = new.unwrap();
        assert!(new.starts_with(text), "the user's text is kept as it is");
        assert_eq!(new.lines().nth(line - 1), Some("# [[user_menu]]"));
        assert_eq!(with_example(&new, ex), (line, None), "not added twice");
        assert_eq!(with_example("", ex), (1, Some(ex.to_string())));
        assert_eq!(with_example("a = 1", ex).0, 3);
        assert_eq!(with_example("x = 1\n[[user_menu]]\nkey = \"y\"\n", ex), (2, None));
        assert_eq!(with_example("user_menu = []\n", ex), (1, None));
    }

    #[test]
    fn user_menu_example_parses_once_uncommented() {
        let ex = example();
        assert!(ex.lines().all(|l| l.starts_with('#')));
        let body = &ex[ex.find("# [[user_menu]]").unwrap()..];
        let open: String = body.lines().map(|l| l.trim_start_matches('#').trim_start()).collect::<Vec<_>>().join("\n");
        let cfg = Config::parse(&open).unwrap();
        assert_eq!(cfg.user_menu.unwrap().last().unwrap().key, "z");
    }

    #[test]
    fn user_menu_editor_at_the_line() {
        assert_eq!(edit_command("vim", Path::new("/c/config.toml"), 7), "vim +7 /c/config.toml");
        assert_eq!(edit_command("/usr/bin/nvim -p", Path::new("/c/config.toml"), 7), "/usr/bin/nvim -p +7 /c/config.toml");
        assert_eq!(edit_command("code --wait", Path::new("/c/config.toml"), 7), "code --wait /c/config.toml");
    }
}
